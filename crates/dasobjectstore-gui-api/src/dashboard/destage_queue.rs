use crate::dashboard::DashboardWarning;
use dasobjectstore_core::ids::{ObjectId, StoreId};
use dasobjectstore_core::lifecycle::ObjectState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DestageQueueView {
    pub pending_objects: usize,
    pub copying_objects: usize,
    pub verified_objects: usize,
    pub objects: Vec<DestageQueueObjectView>,
    pub warnings: Vec<DashboardWarning>,
}

impl DestageQueueView {
    pub fn from_objects(objects: Vec<DestageQueueObjectView>) -> Self {
        let pending_objects = objects
            .iter()
            .filter(|object| object.state.is_pending_destage())
            .count();
        let copying_objects = objects
            .iter()
            .filter(|object| object.state.is_copying())
            .count();
        let verified_objects = objects
            .iter()
            .filter(|object| object.state.is_verified())
            .count();

        let warnings = destage_queue_warnings(&objects);

        Self {
            pending_objects,
            copying_objects,
            verified_objects,
            objects,
            warnings,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DestageQueueObjectView {
    pub object_id: String,
    pub store_id: String,
    pub state: ObjectStateView,
    pub copy_count: usize,
    pub required_copies: u8,
    pub updated_at_utc: String,
    pub warnings: Vec<DashboardWarning>,
}

impl DestageQueueObjectView {
    pub fn from_object(
        object_id: &ObjectId,
        store_id: &StoreId,
        state: ObjectState,
        copy_count: usize,
        required_copies: u8,
        updated_at_utc: impl Into<String>,
    ) -> Self {
        Self {
            object_id: object_id.to_string(),
            store_id: store_id.to_string(),
            state: ObjectStateView::from(state),
            copy_count,
            required_copies,
            updated_at_utc: updated_at_utc.into(),
            warnings: destage_object_warnings(state, copy_count, required_copies),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectStateView {
    ReceivedOnSsd,
    HashVerified,
    PlacementPlanned,
    CopyingToHdd,
    HddCopyVerified,
    Protected,
    SsdEvictionEligible,
    RedownloadRequired,
}

impl ObjectStateView {
    fn is_pending_destage(self) -> bool {
        matches!(
            self,
            Self::ReceivedOnSsd | Self::HashVerified | Self::PlacementPlanned
        )
    }

    fn is_copying(self) -> bool {
        matches!(self, Self::CopyingToHdd)
    }

    fn is_verified(self) -> bool {
        matches!(
            self,
            Self::HddCopyVerified | Self::Protected | Self::SsdEvictionEligible
        )
    }
}

impl From<ObjectState> for ObjectStateView {
    fn from(state: ObjectState) -> Self {
        match state {
            ObjectState::ReceivedOnSsd => Self::ReceivedOnSsd,
            ObjectState::HashVerified => Self::HashVerified,
            ObjectState::PlacementPlanned => Self::PlacementPlanned,
            ObjectState::CopyingToHdd => Self::CopyingToHdd,
            ObjectState::HddCopyVerified => Self::HddCopyVerified,
            ObjectState::Protected => Self::Protected,
            ObjectState::SsdEvictionEligible => Self::SsdEvictionEligible,
            ObjectState::RedownloadRequired => Self::RedownloadRequired,
        }
    }
}

fn destage_queue_warnings(objects: &[DestageQueueObjectView]) -> Vec<DashboardWarning> {
    if objects.iter().any(|object| !object.warnings.is_empty()) {
        vec![DashboardWarning::new(
            "destage_objects_need_review",
            "One or more destage objects need review before SSD eviction.",
        )]
    } else {
        Vec::new()
    }
}

fn destage_object_warnings(
    state: ObjectState,
    copy_count: usize,
    required_copies: u8,
) -> Vec<DashboardWarning> {
    let mut warnings = Vec::new();

    if matches!(state, ObjectState::RedownloadRequired) {
        warnings.push(DashboardWarning::new(
            "object_redownload_required",
            "Object must be redownloaded or regenerated before it is available.",
        ));
    }

    if matches!(
        state,
        ObjectState::HddCopyVerified | ObjectState::Protected | ObjectState::SsdEvictionEligible
    ) && copy_count < usize::from(required_copies)
    {
        warnings.push(DashboardWarning::new(
            "object_under_replicated",
            "Verified copies are below the store redundancy target.",
        ));
    }

    warnings
}

#[cfg(test)]
mod tests {
    use super::{DestageQueueObjectView, DestageQueueView};
    use dasobjectstore_core::ids::{ObjectId, StoreId};
    use dasobjectstore_core::lifecycle::ObjectState;

    #[test]
    fn builds_destage_queue_view_from_objects() {
        let store_id = StoreId::new("store-a").expect("store id");
        let pending = DestageQueueObjectView::from_object(
            &ObjectId::new("object-a").expect("object id"),
            &store_id,
            ObjectState::PlacementPlanned,
            0,
            2,
            "2026-01-05T00:00:00Z",
        );
        let copying = DestageQueueObjectView::from_object(
            &ObjectId::new("object-b").expect("object id"),
            &store_id,
            ObjectState::CopyingToHdd,
            1,
            2,
            "2026-01-05T00:01:00Z",
        );
        let under_replicated = DestageQueueObjectView::from_object(
            &ObjectId::new("object-c").expect("object id"),
            &store_id,
            ObjectState::Protected,
            1,
            2,
            "2026-01-05T00:02:00Z",
        );

        let view = DestageQueueView::from_objects(vec![pending, copying, under_replicated]);

        assert_eq!(view.pending_objects, 1);
        assert_eq!(view.copying_objects, 1);
        assert_eq!(view.verified_objects, 1);
        assert_eq!(view.objects[2].warnings[0].code, "object_under_replicated");
        assert_eq!(view.warnings[0].code, "destage_objects_need_review");
    }

    #[test]
    fn serializes_destage_queue_for_dashboard_contract() {
        let object = DestageQueueObjectView::from_object(
            &ObjectId::new("object-a").expect("object id"),
            &StoreId::new("store-a").expect("store id"),
            ObjectState::SsdEvictionEligible,
            2,
            2,
            "2026-01-05T00:00:00Z",
        );
        let view = DestageQueueView::from_objects(vec![object]);

        let encoded = serde_json::to_value(view).expect("destage queue serializes");

        assert_eq!(encoded["objects"][0]["state"], "ssd_eviction_eligible");
        assert_eq!(encoded["verified_objects"], 1);
        assert_eq!(
            encoded["warnings"]
                .as_array()
                .expect("warnings array")
                .len(),
            0
        );
    }
}
