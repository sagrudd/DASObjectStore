//! HDD placement candidate domain model.

use crate::ids::{DiskId, EnclosureId};
use crate::lifecycle::HealthState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlacementCandidate {
    pub disk_id: DiskId,
    pub enclosure_id: Option<EnclosureId>,
    pub available_bytes: u64,
    pub health_state: HealthState,
    pub performance_class: PerformanceClass,
    pub write_load: WriteLoad,
}

impl PlacementCandidate {
    pub fn new(
        disk_id: DiskId,
        enclosure_id: Option<EnclosureId>,
        available_bytes: u64,
        health_state: HealthState,
        performance_class: PerformanceClass,
        write_load: WriteLoad,
    ) -> Self {
        Self {
            disk_id,
            enclosure_id,
            available_bytes,
            health_state,
            performance_class,
            write_load,
        }
    }

    pub fn has_capacity_for(&self, object_size_bytes: u64) -> bool {
        self.available_bytes >= object_size_bytes
    }

    pub fn accepts_new_protected_copy(&self) -> bool {
        matches!(self.health_state, HealthState::Healthy | HealthState::Watch)
    }

    pub fn is_candidate_for(&self, request: &PlacementRequest) -> bool {
        self.has_capacity_for(request.object_size_bytes)
            && (!request.protected_copy || self.accepts_new_protected_copy())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum PerformanceClass {
    Unknown,
    Slow,
    Standard,
    Fast,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum WriteLoad {
    Idle,
    Light,
    Busy,
    Saturated,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PlacementRequest {
    pub object_size_bytes: u64,
    pub protected_copy: bool,
}

impl PlacementRequest {
    pub fn protected(object_size_bytes: u64) -> Self {
        Self {
            object_size_bytes,
            protected_copy: true,
        }
    }

    pub fn cache(object_size_bytes: u64) -> Self {
        Self {
            object_size_bytes,
            protected_copy: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{PerformanceClass, PlacementCandidate, PlacementRequest, WriteLoad};
    use crate::ids::{DiskId, EnclosureId};
    use crate::lifecycle::HealthState;

    #[test]
    fn candidate_captures_placement_inputs() {
        let candidate = candidate(
            "disk-a",
            Some("enclosure-a"),
            10_000,
            HealthState::Healthy,
            PerformanceClass::Fast,
            WriteLoad::Light,
        );

        assert_eq!(candidate.disk_id.as_str(), "disk-a");
        assert_eq!(
            candidate
                .enclosure_id
                .as_ref()
                .expect("enclosure id")
                .as_str(),
            "enclosure-a"
        );
        assert_eq!(candidate.available_bytes, 10_000);
        assert_eq!(candidate.health_state, HealthState::Healthy);
        assert_eq!(candidate.performance_class, PerformanceClass::Fast);
        assert_eq!(candidate.write_load, WriteLoad::Light);
    }

    #[test]
    fn candidate_requires_sufficient_capacity() {
        let candidate = candidate(
            "disk-a",
            None,
            512,
            HealthState::Healthy,
            PerformanceClass::Standard,
            WriteLoad::Idle,
        );

        assert!(candidate.has_capacity_for(512));
        assert!(!candidate.has_capacity_for(513));
    }

    #[test]
    fn protected_copy_rejects_suspect_or_worse_health() {
        for health_state in [
            HealthState::Suspect,
            HealthState::Draining,
            HealthState::Retired,
            HealthState::Failed,
        ] {
            let candidate = candidate(
                "disk-a",
                None,
                1_000,
                health_state,
                PerformanceClass::Unknown,
                WriteLoad::Idle,
            );

            assert!(!candidate.is_candidate_for(&PlacementRequest::protected(1)));
        }
    }

    #[test]
    fn cache_copy_can_use_watch_or_suspect_disk_when_capacity_exists() {
        let candidate = candidate(
            "disk-a",
            None,
            1_000,
            HealthState::Suspect,
            PerformanceClass::Slow,
            WriteLoad::Busy,
        );

        assert!(candidate.is_candidate_for(&PlacementRequest::cache(1_000)));
    }

    #[test]
    fn round_trips_candidate_json() {
        let candidate = candidate(
            "disk-a",
            Some("enclosure-a"),
            10_000,
            HealthState::Watch,
            PerformanceClass::Standard,
            WriteLoad::Busy,
        );

        let encoded = serde_json::to_string(&candidate).expect("candidate serializes");
        let decoded: PlacementCandidate =
            serde_json::from_str(&encoded).expect("candidate deserializes");

        assert_eq!(decoded, candidate);
    }

    fn candidate(
        disk_id: &str,
        enclosure_id: Option<&str>,
        available_bytes: u64,
        health_state: HealthState,
        performance_class: PerformanceClass,
        write_load: WriteLoad,
    ) -> PlacementCandidate {
        PlacementCandidate::new(
            DiskId::new(disk_id).expect("disk id"),
            enclosure_id.map(|id| EnclosureId::new(id).expect("enclosure id")),
            available_bytes,
            health_state,
            performance_class,
            write_load,
        )
    }
}
