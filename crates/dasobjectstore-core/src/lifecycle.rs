//! Domain lifecycle states.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PoolState {
    New,
    Clean,
    Dirty,
    ReadOnly,
    Repairing,
    Degraded,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DiskState {
    Candidate,
    Healthy,
    Watch,
    Suspect,
    Draining,
    Retired,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StoreState {
    Draft,
    Active,
    ReadOnly,
    Suspended,
    Retired,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum HealthState {
    Healthy,
    Watch,
    Suspect,
    Draining,
    Retired,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepairState {
    NotRequired,
    Pending,
    Running,
    Blocked,
    Complete,
    Failed,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ImportMode {
    ReadWrite,
    ReadOnly,
    Repair,
    ForceReadWrite,
}

#[cfg(test)]
mod tests {
    use super::ObjectState;

    #[test]
    fn permits_expected_object_transition() {
        assert!(ObjectState::ReceivedOnSsd.can_transition_to(ObjectState::HashVerified));
    }

    #[test]
    fn rejects_out_of_order_object_transition() {
        assert!(!ObjectState::ReceivedOnSsd.can_transition_to(ObjectState::Protected));
    }
}
