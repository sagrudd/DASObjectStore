//! Object protection decisions after HDD copy verification.

use crate::ids::DiskId;
use crate::lifecycle::{ObjectState, ObjectStateTransitionError};
use crate::store::StorePolicy;
use std::collections::BTreeSet;
use std::fmt::{self, Display};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedCopy {
    pub disk_id: DiskId,
    pub copy_number: u8,
}

impl VerifiedCopy {
    pub fn new(disk_id: DiskId, copy_number: u8) -> Self {
        Self {
            disk_id,
            copy_number,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ObjectProtectionDecision {
    pub required_copies: u8,
    pub verified_distinct_copies: u8,
    pub next_state: ObjectState,
}

impl ObjectProtectionDecision {
    pub fn is_policy_satisfied(&self) -> bool {
        self.verified_distinct_copies >= self.required_copies
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ObjectProtectionError {
    InvalidCurrentState(ObjectState),
    InvalidTransition(ObjectStateTransitionError),
}

impl Display for ObjectProtectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidCurrentState(state) => write!(
                formatter,
                "object protection can only be evaluated from HddCopyVerified, got {state:?}"
            ),
            Self::InvalidTransition(err) => err.fmt(formatter),
        }
    }
}

impl std::error::Error for ObjectProtectionError {}

impl From<ObjectStateTransitionError> for ObjectProtectionError {
    fn from(err: ObjectStateTransitionError) -> Self {
        Self::InvalidTransition(err)
    }
}

pub fn evaluate_object_protection(
    current_state: ObjectState,
    policy: &StorePolicy,
    verified_copies: &[VerifiedCopy],
) -> Result<ObjectProtectionDecision, ObjectProtectionError> {
    if current_state != ObjectState::HddCopyVerified {
        return Err(ObjectProtectionError::InvalidCurrentState(current_state));
    }

    let verified_distinct_copies = count_distinct_copy_disks(verified_copies);
    let next_state = if verified_distinct_copies >= policy.copies {
        current_state.transition_to(ObjectState::Protected)?
    } else {
        current_state
    };

    Ok(ObjectProtectionDecision {
        required_copies: policy.copies,
        verified_distinct_copies,
        next_state,
    })
}

fn count_distinct_copy_disks(verified_copies: &[VerifiedCopy]) -> u8 {
    verified_copies
        .iter()
        .map(|copy| copy.disk_id.clone())
        .collect::<BTreeSet<_>>()
        .len()
        .min(u8::MAX as usize) as u8
}

#[cfg(test)]
mod tests {
    use super::{evaluate_object_protection, ObjectProtectionError, VerifiedCopy};
    use crate::ids::DiskId;
    use crate::lifecycle::ObjectState;
    use crate::store::{StoreClass, StorePolicy};

    #[test]
    fn marks_object_protected_when_policy_required_copies_are_verified() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        let copies = vec![copy("disk-a", 1), copy("disk-b", 2)];

        let decision = evaluate_object_protection(ObjectState::HddCopyVerified, &policy, &copies)
            .expect("protection decision");

        assert!(decision.is_policy_satisfied());
        assert_eq!(decision.required_copies, 2);
        assert_eq!(decision.verified_distinct_copies, 2);
        assert_eq!(decision.next_state, ObjectState::Protected);
    }

    #[test]
    fn keeps_object_at_hdd_copy_verified_until_policy_is_satisfied() {
        let policy = StorePolicy::defaults_for(StoreClass::CriticalMetadata);
        let copies = vec![copy("disk-a", 1), copy("disk-b", 2)];

        let decision = evaluate_object_protection(ObjectState::HddCopyVerified, &policy, &copies)
            .expect("protection decision");

        assert!(!decision.is_policy_satisfied());
        assert_eq!(decision.required_copies, 3);
        assert_eq!(decision.verified_distinct_copies, 2);
        assert_eq!(decision.next_state, ObjectState::HddCopyVerified);
    }

    #[test]
    fn duplicate_verified_copies_on_same_disk_do_not_satisfy_policy() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        let copies = vec![copy("disk-a", 1), copy("disk-a", 2)];

        let decision = evaluate_object_protection(ObjectState::HddCopyVerified, &policy, &copies)
            .expect("protection decision");

        assert!(!decision.is_policy_satisfied());
        assert_eq!(decision.verified_distinct_copies, 1);
        assert_eq!(decision.next_state, ObjectState::HddCopyVerified);
    }

    #[test]
    fn rejects_protection_evaluation_from_unverified_state() {
        let policy = StorePolicy::defaults_for(StoreClass::GeneratedData);
        let copies = vec![copy("disk-a", 1), copy("disk-b", 2)];

        let err = evaluate_object_protection(ObjectState::CopyingToHdd, &policy, &copies)
            .expect_err("invalid current state");

        assert_eq!(
            err,
            ObjectProtectionError::InvalidCurrentState(ObjectState::CopyingToHdd)
        );
    }

    fn copy(disk_id: &str, copy_number: u8) -> VerifiedCopy {
        VerifiedCopy::new(DiskId::new(disk_id).expect("disk id"), copy_number)
    }
}
