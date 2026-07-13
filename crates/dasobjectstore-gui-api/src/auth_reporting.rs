//! Daemon response projections and stable GUI labels.

use super::*;

pub(super) fn create_local_group_response_from_daemon(
    response: DaemonCreateLocalGroupResponse,
) -> StandaloneLocalGroupAdminResponse {
    let client_request_id = response.accepted.client_request_id.clone();
    StandaloneLocalGroupAdminResponse {
        accepted: standalone_accepted_response_from_daemon(response.accepted),
        operation: StandaloneLocalGroupOperation::CreateGroup,
        group_name: response.group_name,
        username: None,
        client_request_id,
    }
}

pub(super) fn assign_local_user_to_group_response_from_daemon(
    response: DaemonAssignLocalUserToLocalGroupResponse,
) -> StandaloneLocalGroupAdminResponse {
    let client_request_id = response.accepted.client_request_id.clone();
    StandaloneLocalGroupAdminResponse {
        accepted: standalone_accepted_response_from_daemon(response.accepted),
        operation: StandaloneLocalGroupOperation::AddUserToGroup,
        group_name: response.group_name,
        username: Some(response.username),
        client_request_id,
    }
}

pub(super) fn enclosure_prepare_response_from_daemon(
    response: DaemonPrepareEnclosureResponse,
) -> StandaloneEnclosurePrepareResponse {
    StandaloneEnclosurePrepareResponse {
        accepted: StandaloneEnclosurePrepareAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "enclosure_preparation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        ssd_device: response.ssd_device.display().to_string(),
        hdd_devices: response
            .hdd_devices
            .into_iter()
            .map(|device| PrepareEnclosureHddDeviceRequest {
                disk_id: device.disk_id,
                device_path: device.device_path.display().to_string(),
            })
            .collect(),
        mount_root: response.mount_root.display().to_string(),
        filesystem: response.filesystem.to_string(),
        owner: response.owner,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

pub(super) fn create_object_store_response_from_daemon(
    response: DaemonCreateObjectStoreResponse,
) -> StandaloneCreateObjectStoreResponse {
    StandaloneCreateObjectStoreResponse {
        accepted: StandaloneCreateObjectStoreAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "object_store_creation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        store_id: response.store_id,
        store_class: response.store_class,
        required_copies: response.required_copies,
        bucket: response.bucket,
        reader_group: response.reader_group,
        writer_group: response.writer_group,
        ssd_root: response.ssd_root.display().to_string(),
        object_type: response.object_type,
        enclosure_id: response.enclosure_id,
        public: response.public,
        writeable: response.writeable,
        capacity_behavior: response.capacity_behavior,
        retention: response.retention,
        endpoint_export_mode: response.endpoint_export_mode,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

pub(super) fn ingest_policy_response_from_daemon(
    response: DaemonUpdateObjectStoreIngestPolicyResponse,
) -> StandaloneObjectStoreIngestPolicyResponse {
    StandaloneObjectStoreIngestPolicyResponse {
        job_id: response.accepted.job_id.to_string(),
        store_id: response.store_id.to_string(),
        previous_ingest_mode: ingest_mode_label(response.previous_ingest_mode),
        ingest_mode: ingest_mode_label(response.ingest_mode),
        changed: response.changed,
        dry_run: response.accepted.dry_run,
        administrator_actor: response.administrator_actor,
    }
}

pub(super) fn ingest_control_response_from_daemon(
    response: DaemonIngestControlResponse,
) -> IngestControlResponse {
    IngestControlResponse {
        state: match response.state {
            DaemonIngestControlState::Running => "running",
            DaemonIngestControlState::Throttled => "throttled",
            DaemonIngestControlState::Paused => "paused",
        }
        .to_string(),
        changed: response.changed,
        dry_run: response.dry_run,
        reason: response.reason,
    }
}

fn ingest_mode_label(mode: dasobjectstore_core::store::IngestMode) -> String {
    match mode {
        dasobjectstore_core::store::IngestMode::SsdFirst => "ssd_first",
        dasobjectstore_core::store::IngestMode::DirectToHdd => "direct_to_hdd",
    }
    .to_string()
}

pub(super) fn endpoint_inventory_upsert_response_from_daemon(
    response: DaemonUpsertEndpointInventoryResponse,
) -> StandaloneEndpointInventoryUpsertResponse {
    StandaloneEndpointInventoryUpsertResponse {
        accepted: StandaloneEndpointInventoryAcceptedResponse {
            job_id: response.accepted.job_id.to_string(),
            kind: "endpoint_validation".to_string(),
            accepted_at_utc: response.accepted.accepted_at_utc,
            dry_run: response.accepted.dry_run,
        },
        endpoint_id: response.endpoint_id,
        display_name: response.display_name,
        kind: endpoint_kind_label(response.kind).to_string(),
        validation_state: endpoint_validation_state_label(response.validation_state).to_string(),
        registry_path: response.registry_path,
        administrator_actor: response.administrator_actor,
        client_request_id: None,
    }
}

fn endpoint_kind_label(kind: DaemonEndpointKind) -> &'static str {
    match kind {
        DaemonEndpointKind::DasobjectstoreDas => "dasobjectstore_das",
        DaemonEndpointKind::DasobjectstoreNfs => "dasobjectstore_nfs",
        DaemonEndpointKind::S3Compatible => "s3_compatible",
    }
}

fn endpoint_validation_state_label(state: DaemonEndpointValidationState) -> &'static str {
    match state {
        DaemonEndpointValidationState::Draft => "draft",
        DaemonEndpointValidationState::PendingValidation => "pending_validation",
        DaemonEndpointValidationState::Validated => "validated",
        DaemonEndpointValidationState::Degraded => "degraded",
        DaemonEndpointValidationState::Rejected => "rejected",
        DaemonEndpointValidationState::Unknown => "unknown",
    }
}

pub(super) fn admin_job_status_response_from_daemon(
    response: DaemonJobStatusResponse,
) -> StandaloneAdminJobStatusResponse {
    StandaloneAdminJobStatusResponse {
        job: admin_job_summary_from_daemon(response.job),
    }
}

fn admin_job_summary_from_daemon(job: DaemonJobSummary) -> StandaloneAdminJobSummary {
    let percent_complete = job.progress.percent_complete();
    StandaloneAdminJobSummary {
        job_id: job.job_id.to_string(),
        kind: admin_job_kind_label(job.kind).to_string(),
        state: admin_job_state_label(job.state).to_string(),
        progress: admin_job_progress_from_daemon(job.progress),
        percent_complete,
        submitted_at_utc: job.submitted_at_utc,
        updated_at_utc: job.updated_at_utc,
        actor: job.actor,
        failure_message: job.failure_message,
    }
}

fn admin_job_progress_from_daemon(progress: DaemonJobProgress) -> StandaloneAdminJobProgress {
    StandaloneAdminJobProgress {
        stage: progress.stage,
        work_bytes_done: progress.work_bytes_done,
        work_bytes_total: progress.work_bytes_total,
        work_units_done: progress.work_units_done,
        work_units_total: progress.work_units_total,
        message: progress.message,
    }
}

pub(super) fn admin_job_cancel_response_from_daemon(
    response: DaemonJobCancelResponse,
) -> StandaloneAdminJobCancelResponse {
    StandaloneAdminJobCancelResponse {
        job_id: response.job_id.to_string(),
        accepted: response.accepted,
        state: admin_job_state_label(response.state).to_string(),
    }
}

fn admin_job_kind_label(kind: DaemonJobKind) -> &'static str {
    match kind {
        DaemonJobKind::IngestFiles => "ingest_files",
        DaemonJobKind::DirectImport => "direct_import",
        DaemonJobKind::DiskDrain => "disk_drain",
        DaemonJobKind::DiskRetire => "disk_retire",
        DaemonJobKind::DiskReplace => "disk_replace",
        DaemonJobKind::EnclosurePreparation => "enclosure_preparation",
        DaemonJobKind::EndpointValidation => "endpoint_validation",
        DaemonJobKind::ObjectStoreCreation => "object_store_creation",
        DaemonJobKind::Repair => "repair",
        DaemonJobKind::RemoteUpload => "remote_upload",
        DaemonJobKind::ServiceOperation => "service_operation",
        DaemonJobKind::SystemAdministration => "system_administration",
    }
}

fn admin_job_state_label(state: DaemonJobState) -> &'static str {
    match state {
        DaemonJobState::Queued => "queued",
        DaemonJobState::Running => "running",
        DaemonJobState::Waiting => "waiting",
        DaemonJobState::Complete => "complete",
        DaemonJobState::Failed => "failed",
        DaemonJobState::Cancelled => "cancelled",
    }
}

fn standalone_accepted_response_from_daemon(
    accepted: dasobjectstore_daemon::DaemonLocalAdminAcceptedResponse,
) -> StandaloneLocalGroupAdminAcceptedResponse {
    StandaloneLocalGroupAdminAcceptedResponse {
        job_id: accepted.job_id.to_string(),
        kind: standalone_accepted_kind(accepted.command).to_string(),
        accepted_at_utc: accepted.accepted_at_utc,
        dry_run: accepted.dry_run,
    }
}

fn standalone_accepted_kind(command: DaemonLocalAdminCommand) -> &'static str {
    match command {
        DaemonLocalAdminCommand::CreateLocalGroup
        | DaemonLocalAdminCommand::AssignLocalUserToLocalGroup => "system_administration",
    }
}

pub(super) fn standalone_admin_client_error(
    err: dasobjectstore_daemon::DaemonClientError,
) -> StandaloneLocalGroupAdminClientError {
    StandaloneLocalGroupAdminClientError {
        message: err.to_string(),
    }
}

pub(super) fn standalone_enclosure_admin_client_error(
    err: dasobjectstore_daemon::DaemonClientError,
) -> StandaloneEnclosureAdminClientError {
    StandaloneEnclosureAdminClientError {
        message: err.to_string(),
    }
}
