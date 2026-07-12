use super::*;

pub(super) fn emit_reconciliation_progress(
    emit_progress: &mut dyn FnMut(
        DaemonIngestProgressEvent,
    ) -> Result<(), DaemonIngestFilesRuntimeError>,
    request: &StoreRepairRequest,
    message: &str,
) -> Result<(), DaemonIngestFilesRuntimeError> {
    use dasobjectstore_core::ids::IngestJobId;
    emit_progress(DaemonIngestProgressEvent {
        job_id: IngestJobId::new("store-repair-s3-reconcile").expect("static job id"),
        endpoint: request
            .store_id
            .clone()
            .expect("validated reconciliation store"),
        stage: crate::api::DaemonIngestStage::Queued,
        pipeline_stage: Some(crate::api::DaemonIngestPipelineStage::SourceRead),
        work_bytes_done: 0,
        work_bytes_total: None,
        source_bytes_done: None,
        source_bytes_total: None,
        stage_bytes_done: None,
        stage_bytes_total: None,
        files_done: 0,
        files_total: None,
        current_object_id: None,
        ssd_pressure: None,
        telemetry: None,
        active_hdd_transfers: Vec::new(),
        resource_policy: None,
        message: Some(message.to_string()),
    })
}

pub(super) fn reconciliation_job_summary(
    request: &StoreRepairRequest,
    accepted_at_utc: &str,
    actor: Option<String>,
    state: crate::api::DaemonJobState,
    message: impl Into<String>,
) -> Result<crate::api::DaemonJobSummary, String> {
    let store_id = request
        .store_id
        .as_ref()
        .expect("validated reconciliation store");
    let timestamp = accepted_at_utc
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let job_id = crate::api::DaemonJobId::new(format!(
        "store-repair-s3-{}-{}",
        store_id.as_str(),
        timestamp.trim_matches('-').to_ascii_lowercase()
    ))
    .map_err(|error| error.to_string())?;
    let terminal = matches!(state, crate::api::DaemonJobState::Complete);
    Ok(crate::api::DaemonJobSummary {
        job_id,
        kind: crate::api::DaemonJobKind::Repair,
        state,
        progress: crate::api::DaemonJobProgress {
            stage: if terminal {
                "complete".to_string()
            } else {
                "reconciliation".to_string()
            },
            work_bytes_done: u64::from(terminal),
            work_bytes_total: 1,
            work_units_done: u64::from(terminal),
            work_units_total: 1,
            message: Some(message.into()),
        },
        submitted_at_utc: accepted_at_utc.to_string(),
        updated_at_utc: accepted_at_utc.to_string(),
        actor,
        failure_message: None,
    })
}

pub(super) fn reconciliation_registration_report(
    live_sqlite_path: &std::path::Path,
) -> dasobjectstore_metadata::RecoverLiveMetadataReport {
    dasobjectstore_metadata::RecoverLiveMetadataReport {
        metadata_path: live_sqlite_path.to_path_buf(),
        backup_path: None,
        dry_run: false,
        stores_scanned: 1,
        payload_files: 0,
        objects_recovered: 0,
        placements_recovered: 0,
        payload_bytes: 0,
        partial_duplicates_omitted: 0,
        hashes_verified: false,
        warning: "Garage reconciliation registered recovered objects through normal SSD-first ingest; no destructive live-metadata rebuild was needed.".to_string(),
    }
}
