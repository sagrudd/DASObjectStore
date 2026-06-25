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

impl HealthState {
    pub fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (Self::Healthy, Self::Watch)
                | (Self::Healthy, Self::Suspect)
                | (Self::Healthy, Self::Draining)
                | (Self::Healthy, Self::Failed)
                | (Self::Watch, Self::Healthy)
                | (Self::Watch, Self::Suspect)
                | (Self::Watch, Self::Draining)
                | (Self::Watch, Self::Failed)
                | (Self::Suspect, Self::Watch)
                | (Self::Suspect, Self::Draining)
                | (Self::Suspect, Self::Failed)
                | (Self::Draining, Self::Retired)
                | (Self::Draining, Self::Failed)
        )
    }

    pub fn transition_to(self, next: Self) -> Result<Self, HealthStateTransitionError> {
        if self.can_transition_to(next) {
            Ok(next)
        } else {
            Err(HealthStateTransitionError {
                current: self,
                next,
            })
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HealthStateTransitionError {
    pub current: HealthState,
    pub next: HealthState,
}

impl Display for HealthStateTransitionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid health state transition: {:?} -> {:?}",
            self.current, self.next
        )
    }
}

impl std::error::Error for HealthStateTransitionError {}

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
    use super::{HealthState, HealthStateTransitionError, ObjectState, ObjectStateTransitionError};

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

    #[test]
    fn permits_health_state_degradation_and_recovery_path() {
        assert!(HealthState::Healthy.can_transition_to(HealthState::Watch));
        assert!(HealthState::Watch.can_transition_to(HealthState::Suspect));
        assert!(HealthState::Suspect.can_transition_to(HealthState::Draining));
        assert!(HealthState::Draining.can_transition_to(HealthState::Retired));
        assert!(HealthState::Watch.can_transition_to(HealthState::Healthy));
        assert!(HealthState::Suspect.can_transition_to(HealthState::Watch));
    }

    #[test]
    fn permits_health_state_failure_from_active_states() {
        for current in [
            HealthState::Healthy,
            HealthState::Watch,
            HealthState::Suspect,
            HealthState::Draining,
        ] {
            assert!(
                current.can_transition_to(HealthState::Failed),
                "{current:?} should transition to failed"
            );
        }
    }

    #[test]
    fn rejects_invalid_health_state_transitions() {
        let transitions = [
            (HealthState::Healthy, HealthState::Retired),
            (HealthState::Suspect, HealthState::Healthy),
            (HealthState::Draining, HealthState::Healthy),
            (HealthState::Retired, HealthState::Healthy),
            (HealthState::Retired, HealthState::Failed),
            (HealthState::Failed, HealthState::Healthy),
            (HealthState::Failed, HealthState::Retired),
            (HealthState::Healthy, HealthState::Healthy),
        ];

        for (current, next) in transitions {
            assert!(
                !current.can_transition_to(next),
                "{current:?} should not transition to {next:?}"
            );
        }
    }

    #[test]
    fn health_transition_to_returns_error_for_invalid_transition() {
        let err = HealthState::Healthy
            .transition_to(HealthState::Retired)
            .expect_err("invalid transition fails");

        assert_eq!(
            err,
            HealthStateTransitionError {
                current: HealthState::Healthy,
                next: HealthState::Retired
            }
        );
        assert_eq!(
            err.to_string(),
            "invalid health state transition: Healthy -> Retired"
        );
    }
}
