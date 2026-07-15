use super::*;
use crate::api::ProfileBindingOperation;

pub(super) fn daemon_job_summary_from_service_lifecycle(
    response: &DaemonServiceLifecycleResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!("service {:?} completed", response.operation),
    )
}

pub(super) fn daemon_job_summary_from_service_provision(
    response: &DaemonServiceProvisionResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        None,
        format!(
            "provisioned {} store(s), {} bucket(s), {} command(s), credentials issued/reused/rotated {}/{}/{}",
            response.stores,
            response.buckets,
            response.commands,
            response.credentials_issued,
            response.credentials_reused,
            response.credentials_rotated
        ),
    )
}

pub(super) fn daemon_job_summary_from_application_identity_registration(
    response: &ApplicationIdentityRegistrationResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "application identity {} {}",
            response.identity.application_id,
            if response.replaced {
                "replaced"
            } else {
                "registered"
            }
        ),
    )
}

pub(super) fn daemon_job_summary_from_application_key_registration(
    response: &ApplicationKeyRegistrationResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "application key {}/{} {}",
            response.key.application_id,
            response.key.key_id,
            if response.replaced {
                "replaced"
            } else {
                "registered"
            }
        ),
    )
}

pub(super) fn daemon_job_summary_from_application_credential_revocation(
    response: &ApplicationCredentialRevocationResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "application credential {}/{} {}",
            response.application_id,
            response.key_id.as_deref().unwrap_or("identity"),
            if response.revoked {
                "revoked"
            } else {
                "not found"
            }
        ),
    )
}

pub(super) fn daemon_job_summary_from_prepare_enclosure(
    response: &PrepareEnclosureResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "prepared {} landing device and {} HDD device(s)",
            response.ssd_device.display(),
            response.hdd_devices.len()
        ),
    )
}

pub(super) fn daemon_job_summary_from_create_object_store(
    response: &CreateObjectStoreResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        DaemonJobKind::ObjectStoreCreation,
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "ObjectStore {} creation accepted for writer group {}",
            response.store_id, response.writer_group
        ),
    )
}

pub(super) fn daemon_job_summary_from_profile_binding(
    response: &ProfileBindingResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "profile binding {} {} for ObjectStore {}",
            match response.operation {
                ProfileBindingOperation::Create => "created",
                ProfileBindingOperation::Provision => {
                    if response.reused {
                        "reused"
                    } else {
                        "provisioned"
                    }
                }
                ProfileBindingOperation::Adopt => "adopted",
            },
            response.deployment_profile.name(),
            response.store_id
        ),
    )
}

pub(super) fn daemon_job_summary_from_profile_migration(
    response: &ProfileMigrationResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        false,
        Some(response.administrator_actor.clone()),
        format!(
            "profile migration {} verified {} object(s)/{} logical bytes from {} to {}; source retained={}",
            response.migration_id,
            response.verified_object_count,
            response.destination_used_bytes,
            response.source_store_id,
            response.destination_store_id,
            response.source_retained
        ),
    )
}

pub(super) fn daemon_job_summary_from_update_object_store_ingest_policy(
    response: &UpdateObjectStoreIngestPolicyResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        response.accepted.kind.clone(),
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "ObjectStore {} ingest mode changed from {:?} to {:?}",
            response.store_id, response.previous_ingest_mode, response.ingest_mode
        ),
    )
}

pub(super) fn daemon_job_summary_from_endpoint_inventory(
    response: &UpsertEndpointInventoryResponse,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        response.accepted.job_id.clone(),
        DaemonJobKind::EndpointValidation,
        response.accepted.accepted_at_utc.clone(),
        response.accepted.dry_run,
        response.administrator_actor.clone(),
        format!(
            "endpoint {} inventory recorded with validation state {:?}",
            response.endpoint_id, response.validation_state
        ),
    )
}

pub(super) fn daemon_job_summary_from_local_admin(
    accepted: &DaemonLocalAdminAcceptedResponse,
    actor: Option<String>,
) -> DaemonJobSummary {
    daemon_job_summary_from_accepted(
        accepted.job_id.clone(),
        DaemonJobKind::SystemAdministration,
        accepted.accepted_at_utc.clone(),
        accepted.dry_run,
        actor,
        format!(
            "local administrator command {:?} completed",
            accepted.command
        ),
    )
}

fn daemon_job_summary_from_accepted(
    job_id: crate::api::DaemonJobId,
    kind: DaemonJobKind,
    accepted_at_utc: String,
    dry_run: bool,
    actor: Option<String>,
    message: String,
) -> DaemonJobSummary {
    let message = if dry_run {
        format!("dry run: {message}")
    } else {
        message
    };
    DaemonJobSummary {
        job_id,
        kind,
        state: DaemonJobState::Complete,
        progress: DaemonJobProgress {
            stage: "complete".to_string(),
            work_bytes_done: 1,
            work_bytes_total: 1,
            work_units_done: 1,
            work_units_total: 1,
            message: Some(message),
        },
        submitted_at_utc: accepted_at_utc.clone(),
        updated_at_utc: accepted_at_utc,
        actor,
        failure_message: None,
    }
}

pub(super) fn remote_easyconnect_aws_cli_upload_job_request(
    request: RemoteEasyconnectSubmitAwsCliUploadRequest,
    accepted_at_utc: &str,
    actor: Option<String>,
    live_sqlite_path: std::path::PathBuf,
) -> RemoteEasyconnectAwsCliUploadJobRequest {
    RemoteEasyconnectAwsCliUploadJobRequest {
        job_id: request.job_id,
        object_store: request.object_store,
        source_bytes: request.source_bytes,
        policy: request.policy,
        ssd_pressure: request.ssd_pressure,
        program: request.program,
        args: request.args,
        display_args: request.display_args,
        environment: request
            .environment
            .into_iter()
            .map(|variable| (variable.name, variable.value))
            .collect(),
        submitted_at_utc: accepted_at_utc.to_string(),
        started_at_utc: accepted_at_utc.to_string(),
        finished_at_utc: accepted_at_utc.to_string(),
        progress_updated_at_utc: accepted_at_utc.to_string(),
        actor,
        progress_telemetry: request
            .progress_telemetry
            .map(remote_upload_progress_telemetry),
        progress_message: request.progress_message,
        completion: request.completion.map(|completion| {
            crate::runtime::RemoteUploadProviderCompletion {
                upload_id: completion.upload_id,
                provider: completion.provider,
                bucket: completion.bucket,
                object_id: completion.object_id,
                object_version: completion.object_version,
                object_key: completion.object_key,
                expected_checksum: completion.expected_checksum,
                endpoint_url: completion.endpoint_url,
            }
        }),
        live_sqlite_path,
    }
}

fn remote_upload_progress_telemetry(
    telemetry: crate::api::RemoteEasyconnectUploadProgressTelemetry,
) -> RemoteUploadProgressTelemetry {
    RemoteUploadProgressTelemetry {
        source_scan_count: telemetry.source_scan_count,
        staged_bytes: telemetry.staged_bytes,
        s3_bytes_per_second: telemetry.s3_bytes_per_second,
        ssd_queue_depth: telemetry.ssd_queue_depth,
        hdd_landing_queue_depth: telemetry.hdd_landing_queue_depth,
        active_hdd_writers: telemetry.active_hdd_writers,
        verification_state: telemetry.verification_state,
        session_renewal_status: telemetry.session_renewal_status,
    }
}
