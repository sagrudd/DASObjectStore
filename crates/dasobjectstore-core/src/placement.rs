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

    pub fn score_for(&self, request: &PlacementRequest) -> Option<PlacementScore> {
        if !self.is_candidate_for(request) {
            return None;
        }

        let capacity_score = capacity_score(self.available_bytes, request.object_size_bytes);
        let health_score = health_score(self.health_state);
        let performance_score = performance_score(self.performance_class);
        let write_load_score = write_load_score(self.write_load);
        let total = capacity_score
            + health_score as u16
            + performance_score as u16
            + write_load_score as u16;

        Some(PlacementScore {
            disk_id: self.disk_id.clone(),
            total,
            capacity_score,
            health_score,
            performance_score,
            write_load_score,
        })
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

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlacementScore {
    pub disk_id: DiskId,
    pub total: u16,
    pub capacity_score: u16,
    pub health_score: u8,
    pub performance_score: u8,
    pub write_load_score: u8,
}

pub fn score_candidates(
    candidates: &[PlacementCandidate],
    request: &PlacementRequest,
) -> Vec<PlacementScore> {
    let mut scores: Vec<_> = candidates
        .iter()
        .filter_map(|candidate| candidate.score_for(request))
        .collect();
    scores.sort_by(compare_scores);
    scores
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CopyPlan {
    pub requested_copies: u8,
    pub planned_copies: Vec<PlannedCopy>,
}

impl CopyPlan {
    pub fn missing_copies(&self) -> u8 {
        self.requested_copies - self.planned_copies.len() as u8
    }

    pub fn is_complete(&self) -> bool {
        self.missing_copies() == 0
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedCopy {
    pub copy_number: u8,
    pub disk_id: DiskId,
    pub score: PlacementScore,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CopyPlanError {
    UnsupportedCopyCount(u8),
}

pub fn plan_copies(
    candidates: &[PlacementCandidate],
    request: &PlacementRequest,
    requested_copies: u8,
) -> Result<CopyPlan, CopyPlanError> {
    if !(1..=3).contains(&requested_copies) {
        return Err(CopyPlanError::UnsupportedCopyCount(requested_copies));
    }

    let planned_copies = score_candidates(candidates, request)
        .into_iter()
        .take(requested_copies as usize)
        .enumerate()
        .map(|(index, score)| PlannedCopy {
            copy_number: index as u8 + 1,
            disk_id: score.disk_id.clone(),
            score,
        })
        .collect();

    Ok(CopyPlan {
        requested_copies,
        planned_copies,
    })
}

fn compare_scores(left: &PlacementScore, right: &PlacementScore) -> std::cmp::Ordering {
    right
        .total
        .cmp(&left.total)
        .then_with(|| left.disk_id.cmp(&right.disk_id))
}

fn capacity_score(available_bytes: u64, object_size_bytes: u64) -> u16 {
    if object_size_bytes == 0 {
        return 100;
    }

    let capacity_multiple = available_bytes / object_size_bytes;
    let capped = capacity_multiple.min(100);
    capped as u16
}

fn health_score(health_state: HealthState) -> u8 {
    match health_state {
        HealthState::Healthy => 100,
        HealthState::Watch => 70,
        HealthState::Suspect => 20,
        HealthState::Draining | HealthState::Retired | HealthState::Failed => 0,
    }
}

fn performance_score(performance_class: PerformanceClass) -> u8 {
    match performance_class {
        PerformanceClass::Fast => 100,
        PerformanceClass::Standard => 70,
        PerformanceClass::Unknown => 50,
        PerformanceClass::Slow => 25,
    }
}

fn write_load_score(write_load: WriteLoad) -> u8 {
    match write_load {
        WriteLoad::Idle => 100,
        WriteLoad::Light => 70,
        WriteLoad::Busy => 35,
        WriteLoad::Saturated => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        plan_copies, score_candidates, CopyPlanError, PerformanceClass, PlacementCandidate,
        PlacementRequest, WriteLoad,
    };
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

    #[test]
    fn scorer_orders_candidates_by_weighted_inputs() {
        let candidates = vec![
            candidate(
                "disk-slow",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Slow,
                WriteLoad::Busy,
            ),
            candidate(
                "disk-fast",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
        ];

        let scores = score_candidates(&candidates, &PlacementRequest::protected(1_000));

        assert_eq!(scores[0].disk_id.as_str(), "disk-fast");
        assert!(scores[0].total > scores[1].total);
        assert_eq!(scores[0].performance_score, 100);
        assert_eq!(scores[0].write_load_score, 100);
    }

    #[test]
    fn scorer_filters_ineligible_candidates() {
        let candidates = vec![
            candidate(
                "disk-too-small",
                None,
                999,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-suspect",
                None,
                10_000,
                HealthState::Suspect,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-ok",
                None,
                10_000,
                HealthState::Watch,
                PerformanceClass::Standard,
                WriteLoad::Light,
            ),
        ];

        let scores = score_candidates(&candidates, &PlacementRequest::protected(1_000));

        assert_eq!(scores.len(), 1);
        assert_eq!(scores[0].disk_id.as_str(), "disk-ok");
    }

    #[test]
    fn scorer_uses_disk_id_as_stable_tie_breaker() {
        let candidates = vec![
            candidate(
                "disk-b",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-a",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
        ];

        let scores = score_candidates(&candidates, &PlacementRequest::protected(1_000));

        assert_eq!(scores[0].disk_id.as_str(), "disk-a");
        assert_eq!(scores[1].disk_id.as_str(), "disk-b");
    }

    #[test]
    fn copy_planner_supports_one_two_and_three_copies() {
        let candidates = vec![
            candidate(
                "disk-slow",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Slow,
                WriteLoad::Busy,
            ),
            candidate(
                "disk-fast",
                None,
                10_000,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-watch",
                None,
                20_000,
                HealthState::Watch,
                PerformanceClass::Standard,
                WriteLoad::Light,
            ),
        ];
        let request = PlacementRequest::protected(1_000);

        let one_copy = plan_copies(&candidates, &request, 1).expect("one-copy plan");
        let two_copies = plan_copies(&candidates, &request, 2).expect("two-copy plan");
        let three_copies = plan_copies(&candidates, &request, 3).expect("three-copy plan");

        assert!(one_copy.is_complete());
        assert!(two_copies.is_complete());
        assert!(three_copies.is_complete());
        assert_eq!(one_copy.planned_copies.len(), 1);
        assert_eq!(two_copies.planned_copies.len(), 2);
        assert_eq!(three_copies.planned_copies.len(), 3);
        assert_eq!(three_copies.planned_copies[0].copy_number, 1);
        assert_eq!(three_copies.planned_copies[0].disk_id.as_str(), "disk-fast");
        assert_eq!(three_copies.planned_copies[1].copy_number, 2);
        assert_eq!(
            three_copies.planned_copies[1].disk_id.as_str(),
            "disk-watch"
        );
        assert_eq!(three_copies.planned_copies[2].copy_number, 3);
        assert_eq!(three_copies.planned_copies[2].disk_id.as_str(), "disk-slow");
    }

    #[test]
    fn copy_planner_reports_missing_copies_when_candidates_are_insufficient() {
        let candidates = vec![
            candidate(
                "disk-too-small",
                None,
                999,
                HealthState::Healthy,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-suspect",
                None,
                10_000,
                HealthState::Suspect,
                PerformanceClass::Fast,
                WriteLoad::Idle,
            ),
            candidate(
                "disk-ok",
                None,
                10_000,
                HealthState::Watch,
                PerformanceClass::Standard,
                WriteLoad::Light,
            ),
        ];

        let plan =
            plan_copies(&candidates, &PlacementRequest::protected(1_000), 3).expect("partial plan");

        assert!(!plan.is_complete());
        assert_eq!(plan.requested_copies, 3);
        assert_eq!(plan.planned_copies.len(), 1);
        assert_eq!(plan.missing_copies(), 2);
        assert_eq!(plan.planned_copies[0].disk_id.as_str(), "disk-ok");
    }

    #[test]
    fn copy_planner_rejects_unsupported_copy_counts() {
        let candidates = vec![candidate(
            "disk-a",
            None,
            10_000,
            HealthState::Healthy,
            PerformanceClass::Fast,
            WriteLoad::Idle,
        )];

        assert_eq!(
            plan_copies(&candidates, &PlacementRequest::protected(1_000), 0),
            Err(CopyPlanError::UnsupportedCopyCount(0))
        );
        assert_eq!(
            plan_copies(&candidates, &PlacementRequest::protected(1_000), 4),
            Err(CopyPlanError::UnsupportedCopyCount(4))
        );
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
