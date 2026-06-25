use crate::format::{FormatVersion, MetadataArtifact};
use dasobjectstore_core::ids::{DiskId, ObjectId, PlacementId, PoolId, StoreId};
use serde::{Deserialize, Serialize};

pub const PLACEMENT_LOG_FORMAT_VERSION: FormatVersion =
    FormatVersion::new(MetadataArtifact::PlacementLog, 0, 1);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PlacementLogRecord {
    pub format_version: FormatVersion,
    pub pool_id: PoolId,
    pub sequence_number: u64,
    pub recorded_at_utc: String,
    pub event: PlacementLogEvent,
}

impl PlacementLogRecord {
    pub fn new(
        pool_id: PoolId,
        sequence_number: u64,
        recorded_at_utc: impl Into<String>,
        event: PlacementLogEvent,
    ) -> Self {
        Self {
            format_version: PLACEMENT_LOG_FORMAT_VERSION,
            pool_id,
            sequence_number,
            recorded_at_utc: recorded_at_utc.into(),
            event,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event_type", rename_all = "snake_case")]
pub enum PlacementLogEvent {
    PlacementPlanned {
        placement_id: PlacementId,
        object_id: ObjectId,
        store_id: StoreId,
        copy_index: u8,
        disk_id: DiskId,
    },
    CopyVerified {
        placement_id: PlacementId,
        object_id: ObjectId,
        store_id: StoreId,
        copy_index: u8,
        disk_id: DiskId,
        relative_path: String,
        size_bytes: u64,
        content_hash: String,
    },
    CopyInvalidated {
        placement_id: PlacementId,
        reason: String,
    },
    ObjectMarkedRedownloadRequired {
        object_id: ObjectId,
        store_id: StoreId,
        reason: String,
    },
}

#[cfg(test)]
mod tests {
    use super::{PlacementLogEvent, PlacementLogRecord, PLACEMENT_LOG_FORMAT_VERSION};
    use crate::format::MetadataArtifact;
    use dasobjectstore_core::ids::{DiskId, ObjectId, PlacementId, PoolId, StoreId};

    #[test]
    fn placement_log_uses_canonical_format_version() {
        assert_eq!(
            PLACEMENT_LOG_FORMAT_VERSION.artifact,
            MetadataArtifact::PlacementLog
        );
        assert_eq!(PLACEMENT_LOG_FORMAT_VERSION.major, 0);
        assert_eq!(PLACEMENT_LOG_FORMAT_VERSION.minor, 1);
    }

    #[test]
    fn serializes_verified_copy_as_jsonl_record() {
        let record = sample_verified_copy_record();

        let encoded = serde_json::to_value(&record).expect("record serializes");

        assert_eq!(encoded["format_version"]["artifact"], "placement_log");
        assert_eq!(encoded["pool_id"], "pool-a");
        assert_eq!(encoded["sequence_number"], 42);
        assert_eq!(encoded["event"]["event_type"], "copy_verified");
        assert_eq!(encoded["event"]["placement_id"], "placement-a");
        assert_eq!(encoded["event"]["object_id"], "object-a");
        assert_eq!(encoded["event"]["store_id"], "generated-data");
        assert_eq!(encoded["event"]["disk_id"], "disk-a");
        assert_eq!(encoded["event"]["relative_path"], "objects/ab/object-a");
        assert_eq!(encoded["event"]["content_hash"], "sha256:object-a");
    }

    #[test]
    fn round_trips_placement_log_record() {
        let record = sample_verified_copy_record();

        let encoded = serde_json::to_string(&record).expect("record serializes");
        let decoded: PlacementLogRecord =
            serde_json::from_str(&encoded).expect("record deserializes");

        assert_eq!(decoded, record);
    }

    #[test]
    fn serializes_recovery_events_with_stable_event_types() {
        let planned = PlacementLogRecord::new(
            pool_id(),
            1,
            "2026-01-02T00:00:00Z",
            PlacementLogEvent::PlacementPlanned {
                placement_id: placement_id(),
                object_id: object_id(),
                store_id: store_id(),
                copy_index: 0,
                disk_id: disk_id(),
            },
        );
        let invalidated = PlacementLogRecord::new(
            pool_id(),
            2,
            "2026-01-02T00:00:01Z",
            PlacementLogEvent::CopyInvalidated {
                placement_id: placement_id(),
                reason: "checksum mismatch".to_string(),
            },
        );
        let redownload = PlacementLogRecord::new(
            pool_id(),
            3,
            "2026-01-02T00:00:02Z",
            PlacementLogEvent::ObjectMarkedRedownloadRequired {
                object_id: object_id(),
                store_id: store_id(),
                reason: "retired disk".to_string(),
            },
        );

        assert_eq!(
            serde_json::to_value(planned).expect("planned serializes")["event"]["event_type"],
            "placement_planned"
        );
        assert_eq!(
            serde_json::to_value(invalidated).expect("invalidated serializes")["event"]
                ["event_type"],
            "copy_invalidated"
        );
        assert_eq!(
            serde_json::to_value(redownload).expect("redownload serializes")["event"]["event_type"],
            "object_marked_redownload_required"
        );
    }

    fn sample_verified_copy_record() -> PlacementLogRecord {
        PlacementLogRecord::new(
            pool_id(),
            42,
            "2026-01-02T00:00:00Z",
            PlacementLogEvent::CopyVerified {
                placement_id: placement_id(),
                object_id: object_id(),
                store_id: store_id(),
                copy_index: 0,
                disk_id: disk_id(),
                relative_path: "objects/ab/object-a".to_string(),
                size_bytes: 1_048_576,
                content_hash: "sha256:object-a".to_string(),
            },
        )
    }

    fn pool_id() -> PoolId {
        PoolId::new("pool-a").expect("pool id")
    }

    fn placement_id() -> PlacementId {
        PlacementId::new("placement-a").expect("placement id")
    }

    fn object_id() -> ObjectId {
        ObjectId::new("object-a").expect("object id")
    }

    fn store_id() -> StoreId {
        StoreId::new("generated-data").expect("store id")
    }

    fn disk_id() -> DiskId {
        DiskId::new("disk-a").expect("disk id")
    }
}
