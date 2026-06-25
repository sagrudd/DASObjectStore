//! Repair and evacuation planning domain logic.

use crate::ids::{DiskId, ObjectId, StoreId};
use crate::placement::{
    plan_copy_count_for_store, CopyPlan, CopyPlanError, PlacementCandidate, PlacementRequest,
};
use crate::protection::VerifiedCopy;
use crate::store::{CapacityBehavior, StoreClass, StorePolicy};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedObjectCopies {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub object_size_bytes: u64,
    pub policy: StorePolicy,
    pub verified_copies: Vec<VerifiedCopy>,
}

impl ProtectedObjectCopies {
    pub fn new(
        object_id: ObjectId,
        store_id: StoreId,
        object_size_bytes: u64,
        policy: StorePolicy,
        verified_copies: Vec<VerifiedCopy>,
    ) -> Self {
        Self {
            object_id,
            store_id,
            object_size_bytes,
            policy,
            verified_copies,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvacuationPlan {
    pub source_disk_id: DiskId,
    pub tasks: Vec<EvacuationTask>,
    pub blocked_objects: Vec<BlockedEvacuation>,
}

impl EvacuationPlan {
    pub fn is_complete(&self) -> bool {
        self.blocked_objects.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EvacuationTask {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub source_disk_id: DiskId,
    pub replacement_plan: CopyPlan,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlockedEvacuation {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub source_disk_id: DiskId,
    pub missing_replacement_copies: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReproducibleCacheEvacuationPlan {
    pub source_disk_id: DiskId,
    pub tasks: Vec<EvacuationTask>,
    pub redownload_required: Vec<RedownloadRequiredObject>,
}

impl ReproducibleCacheEvacuationPlan {
    pub fn is_fully_copied(&self) -> bool {
        self.redownload_required.is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RedownloadRequiredObject {
    pub object_id: ObjectId,
    pub store_id: StoreId,
    pub source_disk_id: DiskId,
}

pub fn plan_protected_store_evacuation(
    source_disk_id: &DiskId,
    objects: &[ProtectedObjectCopies],
    candidates: &[PlacementCandidate],
) -> Result<EvacuationPlan, CopyPlanError> {
    let mut tasks = Vec::new();
    let mut blocked_objects = Vec::new();

    for object in objects {
        if !object.policy.is_protected_class() {
            continue;
        }

        let replacement_copies = replacement_copy_count(source_disk_id, &object.verified_copies);
        if replacement_copies == 0 {
            continue;
        }

        let request = placement_request_for_existing_copies(object);
        let replacement_plan =
            plan_copy_count_for_store(candidates, &request, &object.policy, replacement_copies)?;

        if !replacement_plan.is_complete() {
            blocked_objects.push(BlockedEvacuation {
                object_id: object.object_id.clone(),
                store_id: object.store_id.clone(),
                source_disk_id: source_disk_id.clone(),
                missing_replacement_copies: replacement_plan.missing_copies(),
            });
        }

        if !replacement_plan.planned_copies.is_empty() {
            tasks.push(EvacuationTask {
                object_id: object.object_id.clone(),
                store_id: object.store_id.clone(),
                source_disk_id: source_disk_id.clone(),
                replacement_plan,
            });
        }
    }

    Ok(EvacuationPlan {
        source_disk_id: source_disk_id.clone(),
        tasks,
        blocked_objects,
    })
}

pub fn plan_reproducible_cache_evacuation(
    source_disk_id: &DiskId,
    objects: &[ProtectedObjectCopies],
    candidates: &[PlacementCandidate],
) -> Result<ReproducibleCacheEvacuationPlan, CopyPlanError> {
    let mut tasks = Vec::new();
    let mut redownload_required = Vec::new();

    for object in objects {
        if !allows_reproducible_cache_redownload(&object.policy) {
            continue;
        }

        let replacement_copies = replacement_copy_count(source_disk_id, &object.verified_copies);
        if replacement_copies == 0 {
            continue;
        }

        let available_candidates =
            candidates_without_existing_copies(candidates, &object.verified_copies);
        let request = PlacementRequest::cache(object.object_size_bytes);
        let replacement_plan = plan_copy_count_for_store(
            &available_candidates,
            &request,
            &object.policy,
            replacement_copies,
        )?;

        if replacement_plan.is_complete() {
            tasks.push(EvacuationTask {
                object_id: object.object_id.clone(),
                store_id: object.store_id.clone(),
                source_disk_id: source_disk_id.clone(),
                replacement_plan,
            });
        } else {
            redownload_required.push(RedownloadRequiredObject {
                object_id: object.object_id.clone(),
                store_id: object.store_id.clone(),
                source_disk_id: source_disk_id.clone(),
            });
        }
    }

    Ok(ReproducibleCacheEvacuationPlan {
        source_disk_id: source_disk_id.clone(),
        tasks,
        redownload_required,
    })
}

fn allows_reproducible_cache_redownload(policy: &StorePolicy) -> bool {
    policy.class == StoreClass::ReproducibleCache
        && policy.capacity_behavior == CapacityBehavior::MarkRedownloadRequired
}

fn candidates_without_existing_copies(
    candidates: &[PlacementCandidate],
    verified_copies: &[VerifiedCopy],
) -> Vec<PlacementCandidate> {
    candidates
        .iter()
        .filter(|candidate| {
            verified_copies
                .iter()
                .all(|copy| copy.disk_id != candidate.disk_id)
        })
        .cloned()
        .collect()
}

fn replacement_copy_count(source_disk_id: &DiskId, verified_copies: &[VerifiedCopy]) -> u8 {
    verified_copies
        .iter()
        .filter(|copy| &copy.disk_id == source_disk_id)
        .count()
        .min(u8::MAX as usize) as u8
}

fn placement_request_for_existing_copies(object: &ProtectedObjectCopies) -> PlacementRequest {
    object.verified_copies.iter().fold(
        PlacementRequest::protected(object.object_size_bytes),
        |request, copy| request.with_existing_copy_on(copy.disk_id.clone()),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        plan_protected_store_evacuation, plan_reproducible_cache_evacuation, ProtectedObjectCopies,
    };
    use crate::ids::{DiskId, EnclosureId, ObjectId, StoreId};
    use crate::lifecycle::HealthState;
    use crate::placement::{PerformanceClass, PlacementCandidate, WriteLoad};
    use crate::protection::VerifiedCopy;
    use crate::store::{StoreClass, StorePolicy};

    #[test]
    fn plans_replacement_copy_for_protected_object_on_source_disk() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
            vec![copy("disk-a", 1), copy("disk-b", 2)],
        )];
        let candidates = vec![
            candidate("disk-a", HealthState::Suspect),
            candidate("disk-b", HealthState::Healthy),
            candidate("disk-c", HealthState::Healthy),
        ];

        let plan = plan_protected_store_evacuation(&source_disk_id, &objects, &candidates)
            .expect("evacuation plan");

        assert!(plan.is_complete());
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(plan.tasks[0].object_id.as_str(), "object-a");
        assert_eq!(plan.tasks[0].replacement_plan.requested_copies, 1);
        assert_eq!(
            plan.tasks[0].replacement_plan.planned_copies[0]
                .disk_id
                .as_str(),
            "disk-c"
        );
    }

    #[test]
    fn ignores_reproducible_cache_for_protected_evacuation() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            vec![copy("disk-a", 1)],
        )];
        let candidates = vec![candidate("disk-b", HealthState::Healthy)];

        let plan = plan_protected_store_evacuation(&source_disk_id, &objects, &candidates)
            .expect("evacuation plan");

        assert!(plan.tasks.is_empty());
        assert!(plan.blocked_objects.is_empty());
    }

    #[test]
    fn reports_blocked_object_when_replacement_capacity_is_unavailable() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
            vec![copy("disk-a", 1), copy("disk-b", 2)],
        )];
        let candidates = vec![candidate("disk-b", HealthState::Healthy)];

        let plan = plan_protected_store_evacuation(&source_disk_id, &objects, &candidates)
            .expect("evacuation plan");

        assert!(!plan.is_complete());
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.blocked_objects.len(), 1);
        assert_eq!(plan.blocked_objects[0].missing_replacement_copies, 1);
    }

    #[test]
    fn plans_opportunistic_copy_for_reproducible_cache_object() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            vec![copy("disk-a", 1)],
        )];
        let candidates = vec![
            candidate("disk-a", HealthState::Suspect),
            candidate("disk-b", HealthState::Suspect),
        ];

        let plan = plan_reproducible_cache_evacuation(&source_disk_id, &objects, &candidates)
            .expect("cache evacuation plan");

        assert!(plan.is_fully_copied());
        assert_eq!(plan.tasks.len(), 1);
        assert_eq!(
            plan.tasks[0].replacement_plan.planned_copies[0]
                .disk_id
                .as_str(),
            "disk-b"
        );
        assert!(plan.redownload_required.is_empty());
    }

    #[test]
    fn marks_reproducible_cache_object_redownload_required_when_copy_is_unavailable() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::ReproducibleCache),
            vec![copy("disk-a", 1)],
        )];
        let candidates = vec![candidate("disk-a", HealthState::Suspect)];

        let plan = plan_reproducible_cache_evacuation(&source_disk_id, &objects, &candidates)
            .expect("cache evacuation plan");

        assert!(!plan.is_fully_copied());
        assert!(plan.tasks.is_empty());
        assert_eq!(plan.redownload_required.len(), 1);
        assert_eq!(plan.redownload_required[0].object_id.as_str(), "object-a");
    }

    #[test]
    fn ignores_protected_store_in_reproducible_cache_evacuation() {
        let source_disk_id = disk("disk-a");
        let objects = vec![protected_object(
            "object-a",
            StorePolicy::defaults_for(StoreClass::GeneratedData),
            vec![copy("disk-a", 1), copy("disk-b", 2)],
        )];
        let candidates = vec![candidate("disk-c", HealthState::Healthy)];

        let plan = plan_reproducible_cache_evacuation(&source_disk_id, &objects, &candidates)
            .expect("cache evacuation plan");

        assert!(plan.tasks.is_empty());
        assert!(plan.redownload_required.is_empty());
    }

    fn protected_object(
        object_id: &str,
        policy: StorePolicy,
        verified_copies: Vec<VerifiedCopy>,
    ) -> ProtectedObjectCopies {
        ProtectedObjectCopies::new(
            ObjectId::new(object_id).expect("object id"),
            StoreId::new("store-a").expect("store id"),
            1_000,
            policy,
            verified_copies,
        )
    }

    fn copy(disk_id: &str, copy_number: u8) -> VerifiedCopy {
        VerifiedCopy::new(disk(disk_id), copy_number)
    }

    fn disk(disk_id: &str) -> DiskId {
        DiskId::new(disk_id).expect("disk id")
    }

    fn candidate(disk_id: &str, health_state: HealthState) -> PlacementCandidate {
        PlacementCandidate::new(
            disk(disk_id),
            Some(EnclosureId::new(format!("enclosure-{disk_id}")).expect("enclosure id")),
            1_000,
            health_state,
            PerformanceClass::Standard,
            WriteLoad::Idle,
        )
    }
}
