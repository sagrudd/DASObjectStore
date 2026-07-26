//! Portable metadata boundary for DASObjectStore pools.

pub mod assurance;
pub mod attach;
pub mod capacity;
pub mod contents;
pub mod copy;
pub mod destage;
pub mod direct_import;
pub mod disk;
pub mod disk_capacity;
pub mod drain;
pub mod evacuation;
pub mod export;
pub mod format;
mod hash;
pub mod ingest;
pub mod initialize;
pub mod inspect;
pub mod integrity;
pub mod local_object_store;
pub mod manifest;
pub mod markers;
pub mod object;
pub mod object_commit;
pub mod placement_log;
pub mod profile_catalogue_commit;
pub mod queue;
pub mod recovery;
pub mod remote_inventory;
pub mod repair_activity;
pub mod s3_access;
pub mod schema;
mod secure_fs;
pub mod snapshot;
pub mod store;
pub mod workspace;
pub mod workspace_attachments;
pub mod workspace_checkpoints;
pub mod workspace_materializations;
pub mod workspace_operations;
pub mod workspace_promotions;

pub use assurance::{
    assurance_primary_work_pending, commit_assurance_relocation, list_assurance_disk_states,
    list_assurance_placement_candidates, record_assurance_hash_failure,
    record_assurance_verification, AssuranceDiskState, AssuranceMetadataError,
    AssurancePlacementCandidate,
};
pub use attach::{
    attach_clean_pool_read_only, import_dirty_pool_read_only, ReadOnlyAttachError,
    ReadOnlyAttachOptions, ReadOnlyAttachReport,
};
pub use capacity::{
    measure_ssd_capacity, SsdCapacity, SsdCapacityMeasurementError, SsdCapacityPolicy,
    SsdCapacityPolicyError, SsdPressure, DEFAULT_SSD_CRITICAL_WATERMARK_PERCENT,
    DEFAULT_SSD_HIGH_WATERMARK_PERCENT,
};
pub use contents::{
    read_store_contents, StoreContentsObject, StoreContentsReadError, StoreContentsRequest,
    StoreContentsSnapshot,
};
pub use copy::{
    verify_hdd_copy_hash, write_hdd_copy_with_inline_hash,
    write_hdd_copy_with_inline_hash_with_controlled_progress, write_verified_hdd_copy,
    write_verified_hdd_copy_with_controlled_progress, write_verified_hdd_copy_with_progress,
    HddCopyError, HddCopyReport, HddCopyRequest, HddInlineHashCopyRequest,
    HDD_COPY_CONTENT_HASH_ALGORITHM,
};
pub use destage::{
    cancel_destage, claim_next_destage, commit_verified_ssd_and_enqueue, destage_queue_diagnostics,
    fail_destage, list_destage_queue, list_ssd_eviction_candidates, mark_ssd_evicted,
    pause_destage, promote_hdd_settlement, read_destage, read_ssd_placement, resume_destage,
    retry_destage, DestageMetadataError, DestageQueueDiagnostics, DestageQueueRecord, DestageState,
    HddSettlementPromotionRequest, SsdPlacementRecord, VerifiedHddPlacement,
    VerifiedSsdCommitReport, VerifiedSsdCommitRequest,
};
pub use direct_import::{
    import_reproducible_object_direct_to_hdd, DirectHddImportError, DirectHddImportReport,
    DirectHddImportRequest,
};
pub use disk::{
    force_retire_disk, request_disk_retirement, DiskRetirementError, DiskRetirementReport,
};
pub use disk_capacity::{
    acquire_disk_capacity_claims, read_outstanding_disk_capacity,
    read_outstanding_disk_capacity_excluding, release_disk_capacity_claims,
    update_disk_capacity_claim_consumption, DiskCapacityClaim, DiskCapacityClaimAllocation,
    DiskCapacityClaimError, DiskCapacityClaimKind, DiskCapacityClaimRequest,
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
pub use hash::{hash_file_sha256, hash_file_sha256_with_progress, SHA256_ALGORITHM};
pub use ingest::{
    IngestJobPaths, IngestJournalChecksumManifest, IngestJournalContentHash,
    IngestJournalFileRecord, IngestJournalFileState, IngestJournalFinalizationReadiness,
    IngestJournalHddWrite, IngestJournalPartialHddWrite, IngestJournalResumeAction,
    IngestJournalResumePlan, IngestJournalTransitionError, IngestPayloadWriteError,
    IngestStagingLayout, IngestWriteReport, INGEST_DIR_NAME, INGEST_JOBS_DIR_NAME,
    INGEST_PAYLOAD_FILE_NAME, INGEST_SCRATCH_DIR_NAME,
};
pub use initialize::{
    initialize_pool, MetadataInitError, PoolInitOptions, PoolInitReport, LIVE_SQLITE_FILE_NAME,
    METADATA_DIR_NAME, SNAPSHOT_DIR_NAME,
};
pub use inspect::{inspect_pool_metadata, PoolInspectError, PoolInspectSummary};
pub use integrity::{
    deduplicate_live_metadata, verify_live_metadata, DeduplicateLiveMetadataError,
    DeduplicateLiveMetadataReport, DeduplicateLiveMetadataRequest, VerifyLiveMetadataError,
    VerifyLiveMetadataReport, VerifyLiveMetadataRequest,
};
pub use local_object_store::{
    adopt_object_on_ssd_by_hard_link_with_controlled_progress,
    existing_object_payload_candidate_paths, object_payload_path,
    put_object_direct_to_hdd_with_controlled_progress, put_object_ssd_first,
    put_object_ssd_first_with_controlled_progress, put_object_ssd_first_with_progress,
    settle_staged_object_to_hdd_preserving_ssd_with_controlled_progress,
    settle_staged_object_to_hdd_with_controlled_progress,
    stage_object_on_ssd_with_controlled_progress, DirectObjectPutRequest, ObjectPutError,
    ObjectPutPlacementReport, ObjectPutProgress, ObjectPutProgressStage, ObjectPutReport,
    ObjectPutRequest, StagedObjectPut,
};
pub use manifest::{
    ArtifactReference, DiskManifest, DiskManifestEntry, PoolManifest, DISK_MANIFEST_FORMAT_VERSION,
    POOL_MANIFEST_FORMAT_VERSION,
};
pub use markers::{
    record_pool_state_marker, record_pool_state_marker_at, PoolStateMarker, PoolStateMarkerKind,
};
pub use object::{
    read_object_inspect, read_store_object_inspects, ObjectInspectError, ObjectInspectSummary,
    ObjectPlacementSummary,
};
pub use object_commit::{commit_object_put, ObjectMetadataCommitError};
pub use placement_log::{PlacementLogEvent, PlacementLogRecord, PLACEMENT_LOG_FORMAT_VERSION};
pub use profile_catalogue_commit::{
    commit_profile_catalogue, profile_catalogue_object_matches, profile_catalogue_snapshot_matches,
    withdraw_profile_catalogue, withdraw_profile_catalogue_object, ProfileCatalogueCommitError,
    ProfileCatalogueCommitReport, ProfileCatalogueCommitRequest,
    ProfileCatalogueObjectWithdrawalReport, ProfileCatalogueObjectWithdrawalRequest,
    ProfileCatalogueWithdrawalReport, PROFILE_CATALOGUE_SCHEMA_VERSION,
};
pub use queue::{
    drain_ingest_queue, read_ingest_queue, read_ingest_queue_for_store, DestagePriorityPolicy,
    DestageUrgency, IngestAdmission, IngestBackpressurePolicy, IngestQueueDrainError,
    IngestQueueDrainReport, IngestQueueDrainRequest, IngestQueueEntry, IngestQueueJob,
    IngestQueuePlan, IngestQueueReadError, IngestQueueSnapshot,
    DEFAULT_CRITICAL_WATERMARK_MINIMUM_PRIORITY, DEFAULT_HIGH_WATERMARK_MINIMUM_PRIORITY,
};
pub use recovery::{
    recover_live_metadata, RecoverLiveMetadataError, RecoverLiveMetadataReport,
    RecoverLiveMetadataRequest, RecoveryStoreDefinition,
};
pub use remote_inventory::{
    read_remote_object_inventory_page, RemoteObjectInventoryError, RemoteObjectInventoryPage,
    RemoteObjectInventoryRecord,
};
pub use repair_activity::{
    read_pool_repair_activity, PoolRepairActivityEvent, PoolRepairActivityReadError,
    PoolRepairActivitySnapshot,
};
pub use s3_access::{
    backfill_s3_object_bindings, list_s3_object_bindings, read_s3_object_binding,
    store_has_s3_object_bindings, S3AccessError, S3BindingBackfillReport, S3ObjectBinding,
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
pub use workspace::{
    list_workspace_reservations, publish_workspace_aggregate_ready, read_workspace_reservation,
    reserve_workspace, transition_workspace, MeasuredWorkspaceDisk, ReserveWorkspaceRequest,
    WorkspaceDiskAllocation, WorkspaceMetadataError, WorkspaceReservationSnapshot,
};
pub use workspace_attachments::{
    list_workspace_attachments, publish_workspace_attachment_state, WorkspaceAttachmentSnapshot,
};
pub use workspace_checkpoints::{
    read_workspace_health, register_workspace_checkpoint, RegisterWorkspaceCheckpoint,
    WorkspaceCapacityReport, WorkspaceCheckpointMember, WorkspaceCheckpointSnapshot,
    WorkspaceHealthReport,
};
pub use workspace_materializations::{
    finish_workspace_materialization, list_active_workspace_materializations,
    publish_workspace_materialization_state, register_workspace_materialization,
    WorkspaceMaterializationSnapshot,
};
pub use workspace_operations::{
    checkpoint_workspace_operation, claim_workspace_operation, finish_workspace_operation,
    list_workspace_operations, read_workspace_operation, recover_expired_workspace_operations,
    renew_workspace_operation_lease, request_workspace_operation_cancellation,
    submit_workspace_operation, SubmitWorkspaceOperationRequest,
    WorkspaceOperationCheckpointSummary, WorkspaceOperationError, WorkspaceOperationRecoveryAction,
    WorkspaceOperationRecoveryRecord, WorkspaceOperationSnapshot,
};
pub use workspace_promotions::{
    accept_workspace_promotion_member, cancel_workspace_promotion, complete_workspace_promotion,
    list_active_workspace_promotions, register_workspace_promotion,
    workspace_promotion_manifest_digest, RegisterWorkspacePromotion,
    WorkspacePromotionMemberRequest, WorkspacePromotionMemberSnapshot, WorkspacePromotionSnapshot,
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
