//! Portable metadata boundary for DASObjectStore pools.

pub mod attach;
pub mod capacity;
pub mod copy;
pub mod direct_import;
pub mod disk;
pub mod drain;
pub mod evacuation;
pub mod export;
pub mod format;
mod hash;
pub mod ingest;
pub mod initialize;
pub mod inspect;
pub mod local_object_store;
pub mod manifest;
pub mod markers;
pub mod object;
pub mod placement_log;
pub mod queue;
pub mod schema;
mod secure_fs;
pub mod snapshot;
pub mod store;

pub use attach::{
    attach_clean_pool_read_only, import_dirty_pool_read_only, ReadOnlyAttachError,
    ReadOnlyAttachOptions, ReadOnlyAttachReport,
};
pub use capacity::{
    measure_ssd_capacity, SsdCapacity, SsdCapacityMeasurementError, SsdCapacityPolicy,
    SsdCapacityPolicyError, SsdPressure, DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT,
    DEFAULT_SSD_HIGH_WATERMARK_PERCENT,
};
pub use copy::{
    verify_hdd_copy_hash, write_verified_hdd_copy, write_verified_hdd_copy_with_progress,
    HddCopyError, HddCopyReport, HddCopyRequest, HDD_COPY_CONTENT_HASH_ALGORITHM,
};
pub use direct_import::{
    import_reproducible_object_direct_to_hdd, DirectHddImportError, DirectHddImportReport,
    DirectHddImportRequest,
};
pub use disk::{
    force_retire_disk, request_disk_retirement, DiskRetirementError, DiskRetirementReport,
};
pub use drain::{
    read_disk_drain_plan, read_disk_replacement_plan, DiskDrainAction, DiskDrainError,
    DiskDrainObjectSummary, DiskDrainPlanSummary, DiskReplacementPlanSummary,
};
pub use evacuation::{
    execute_evacuation_plan, DiskCopyRoot, EvacuationExecutionError, EvacuationExecutionReport,
    EvacuationExecutionRequest, EvacuationObjectSource,
};
pub use export::{
    export_settled_object, ObjectExportError, ObjectExportReport, ObjectExportRequest,
};
pub use format::{FormatVersion, MetadataArtifact};
pub use ingest::{
    IngestJobPaths, IngestJournalChecksumManifest, IngestJournalContentHash,
    IngestJournalFileRecord, IngestJournalFileState, IngestJournalFinalizationReadiness,
    IngestJournalHddWrite, IngestJournalPartialHddWrite, IngestJournalResumeAction,
    IngestJournalResumePlan, IngestJournalTransitionError, IngestStagingLayout, INGEST_DIR_NAME,
    INGEST_JOBS_DIR_NAME, INGEST_PAYLOAD_FILE_NAME, INGEST_SCRATCH_DIR_NAME,
};
pub use initialize::{
    initialize_pool, MetadataInitError, PoolInitOptions, PoolInitReport, LIVE_SQLITE_FILE_NAME,
    METADATA_DIR_NAME, SNAPSHOT_DIR_NAME,
};
pub use inspect::{inspect_pool_metadata, PoolInspectError, PoolInspectSummary};
pub use local_object_store::{
    put_object_ssd_first, put_object_ssd_first_with_controlled_progress,
    put_object_ssd_first_with_progress, ObjectPutError, ObjectPutPlacementReport,
    ObjectPutProgress, ObjectPutProgressStage, ObjectPutReport, ObjectPutRequest,
};
pub use manifest::{
    ArtifactReference, DiskManifest, DiskManifestEntry, PoolManifest, DISK_MANIFEST_FORMAT_VERSION,
    POOL_MANIFEST_FORMAT_VERSION,
};
pub use markers::{
    record_pool_state_marker, record_pool_state_marker_at, PoolStateMarker, PoolStateMarkerKind,
};
pub use object::{
    read_object_inspect, ObjectInspectError, ObjectInspectSummary, ObjectPlacementSummary,
};
pub use placement_log::{PlacementLogEvent, PlacementLogRecord, PLACEMENT_LOG_FORMAT_VERSION};
pub use queue::{
    drain_ingest_queue, read_ingest_queue, read_ingest_queue_for_store, DestagePriorityPolicy,
    DestageUrgency, IngestAdmission, IngestBackpressurePolicy, IngestQueueDrainError,
    IngestQueueDrainReport, IngestQueueDrainRequest, IngestQueueEntry, IngestQueueJob,
    IngestQueuePlan, IngestQueueReadError, IngestQueueSnapshot,
    DEFAULT_CRITICAL_WATERMARK_MINIMUM_PRIORITY, DEFAULT_HIGH_WATERMARK_MINIMUM_PRIORITY,
};
pub use schema::{LIVE_SCHEMA_FORMAT_VERSION, LIVE_SCHEMA_SQL};
pub use snapshot::{
    export_metadata_snapshot, import_metadata_snapshot, SnapshotExportError, SnapshotExportOptions,
    SnapshotExportReport, SnapshotImportError, SnapshotImportOptions, SnapshotImportReport,
    DISK_MANIFEST_FILE_NAME, PLACEMENT_LOG_FILE_NAME, POOL_MANIFEST_FILE_NAME,
};
pub use store::{
    delete_store, drain_store, StoreCleanupError, StoreDeleteReport, StoreDeleteRequest,
    StoreDrainReport, StoreDrainRequest, StorePayloadRemoval,
};

/// Returns the metadata crate version.
pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    #[test]
    fn exposes_package_version() {
        assert_eq!(super::version(), env!("CARGO_PKG_VERSION"));
    }
}
